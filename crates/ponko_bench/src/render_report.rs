//! The rendering baseline's results: what the scene runner measured, as
//! plain data, and its JSON and CSV forms. No GPU in here, so the formats
//! are tested everywhere; `render_scenes` fills it in behind the `render`
//! feature. `RENDER_SCENES.md` documents the fields.
//!
//! A metric the machine can't measure is never left out or written as 0:
//! its value is `null` (empty in CSV) and a reason says why.

use std::fmt::Write as _;

use serde_json::{Value, json};

/// Bumped when a field of the `render` section changes meaning or
/// disappears. New fields don't bump it.
pub const RENDER_SCHEMA_VERSION: u32 = 1;

/// How a set of samples is spread. Times are in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dist {
    pub samples: usize,
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

impl Dist {
    /// Summarizes `values` (sorted in place), nearest-rank percentiles as
    /// in the other benchmarks. `None` without samples.
    pub fn of(values: &mut [f64]) -> Option<Dist> {
        if values.is_empty() {
            return None;
        }
        values.sort_by(f64::total_cmp);
        let rank = |pct: usize| values[(pct * values.len()).div_ceil(100).max(1) - 1];
        Some(Dist {
            samples: values.len(),
            min: values[0],
            p50: rank(50),
            p95: rank(95),
            p99: rank(99),
            max: values[values.len() - 1],
            mean: values.iter().sum::<f64>() / values.len() as f64,
        })
    }

    fn json(&self) -> Value {
        json!({
            "samples": self.samples,
            "min": round(self.min),
            "p50": round(self.p50),
            "p95": round(self.p95),
            "p99": round(self.p99),
            "max": round(self.max),
            "mean": round(self.mean),
        })
    }
}

/// Six decimals: nanoseconds when the unit is milliseconds.
fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// One measured frame, for the per-frame CSV.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameRow {
    pub update_ms: f64,
    pub collect_ms: f64,
    pub prepare_ms: f64,
    pub encode_ms: f64,
    pub submit_ms: f64,
    pub frame_ms: f64,
    /// `None`: this frame's GPU passes weren't timed.
    pub gpu_ms: Option<f64>,
    pub gpu_span_ms: Option<f64>,
    pub draw_calls: u64,
    pub recorded_draws: u64,
    pub instances: u64,
    pub triangles: u64,
    pub shadow_instances: u64,
    pub shadow_triangles: u64,
    pub occluded: u64,
    pub upload_bytes: u64,
    pub buffers_created: u64,
    pub pipelines_built: u64,
}

impl FrameRow {
    /// The renderer's CPU time: collect, prepare, encode and submit.
    pub fn cpu_ms(&self) -> f64 {
        self.collect_ms + self.prepare_ms + self.encode_ms + self.submit_ms
    }
}

/// GPU times of one scene.
#[derive(Clone, Debug, Default)]
pub struct GpuTimes {
    /// Measured frames whose passes were timed.
    pub timed_frames: usize,
    /// The passes' times added up, per frame.
    pub total: Option<Dist>,
    /// From the first pass's start to the last one's end, per frame.
    pub span: Option<Dist>,
    /// Each pass, in the order of the last timed frame.
    pub passes: Vec<(String, Dist)>,
    /// Why there are no GPU times, when there are none.
    pub unavailable: Option<String>,
}

/// One scene's results.
#[derive(Clone, Debug, Default)]
pub struct SceneReport {
    pub name: String,
    /// `SceneBench::scene_json`: description, content and quality settings.
    pub scene_json: String,
    /// `SceneBench::memory_json`.
    pub memory_json: String,
    pub warmup: usize,
    pub frames: Vec<FrameRow>,
    pub gpu: GpuTimes,
    /// Mean bytes uploaded per frame, by kind.
    pub uploads_per_frame: Vec<(&'static str, f64)>,
    /// Buffers replaced by bigger ones during the measured frames.
    pub reallocations: u64,
    /// How the instance batches are routed, from `InstanceBatchStats`.
    pub instance_batches: Vec<(&'static str, u64)>,
    /// Pipelines, bind groups, vertex and index buffers set in the last frame.
    pub state_changes: [u64; 4],
    pub command_buffers: u64,
    /// Heap allocations (count and bytes) per measured frame, whole process.
    pub allocs_per_frame: f64,
    pub alloc_bytes_per_frame: f64,
}

/// A column of the frame rows: its name and how to read it.
type Column<T> = (&'static str, fn(&FrameRow) -> T);

/// The timing columns of a scene, by name.
const TIMINGS: [Column<f64>; 7] = [
    ("update", |f| f.update_ms),
    ("collect", |f| f.collect_ms),
    ("prepare", |f| f.prepare_ms),
    ("encode", |f| f.encode_ms),
    ("submit", |f| f.submit_ms),
    ("cpu", FrameRow::cpu_ms),
    ("frame", |f| f.frame_ms),
];

/// The count columns of a scene, by name.
const COUNTS: [Column<u64>; 10] = [
    ("draw_calls", |f| f.draw_calls),
    ("recorded_draws", |f| f.recorded_draws),
    ("instances", |f| f.instances),
    ("triangles", |f| f.triangles),
    ("shadow_instances", |f| f.shadow_instances),
    ("shadow_triangles", |f| f.shadow_triangles),
    ("occluded", |f| f.occluded),
    ("upload_bytes", |f| f.upload_bytes),
    ("buffers_created", |f| f.buffers_created),
    ("pipelines_built", |f| f.pipelines_built),
];

impl SceneReport {
    /// A timing's spread over the measured frames, by its name in `TIMINGS`.
    pub fn timing(&self, name: &str) -> Option<Dist> {
        let (_, get) = TIMINGS.iter().find(|(n, _)| *n == name)?;
        Dist::of(&mut self.frames.iter().map(get).collect::<Vec<_>>())
    }

    fn count(&self, get: fn(&FrameRow) -> u64) -> Option<Dist> {
        Dist::of(&mut self.frames.iter().map(|f| get(f) as f64).collect::<Vec<_>>())
    }

    fn json(&self) -> Value {
        let parsed = |text: &str| serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
        let timings: serde_json::Map<String, Value> = TIMINGS
            .iter()
            .filter_map(|(name, _)| Some(((*name).to_owned(), self.timing(name)?.json())))
            .collect();
        let counts: serde_json::Map<String, Value> = COUNTS
            .iter()
            .filter_map(|&(name, get)| {
                let d = self.count(get)?;
                Some((
                    name.to_owned(),
                    json!({"min": d.min, "p50": d.p50, "max": d.max, "mean": round(d.mean)}),
                ))
            })
            .collect();
        let passes: Vec<Value> = self
            .gpu
            .passes
            .iter()
            .map(|(name, d)| {
                let mut pass = d.json();
                pass["name"] = json!(name);
                pass
            })
            .collect();
        let uploads: serde_json::Map<String, Value> = self
            .uploads_per_frame
            .iter()
            .map(|&(kind, bytes)| (kind.to_owned(), json!(round(bytes))))
            .collect();
        let batches: serde_json::Map<String, Value> = self
            .instance_batches
            .iter()
            .map(|&(key, value)| (key.to_owned(), json!(value)))
            .collect();
        let [pipelines, bind_groups, vertex_buffers, index_buffers] = self.state_changes;
        json!({
            "name": self.name,
            "scene": parsed(&self.scene_json),
            "warmup_frames": self.warmup,
            "frames": self.frames.len(),
            "timings_ms": timings,
            "gpu_ms": {
                "timed_frames": self.gpu.timed_frames,
                "total": self.gpu.total.map(|d| d.json()),
                "span": self.gpu.span.map(|d| d.json()),
                "passes": passes,
                "unavailable": self.gpu.unavailable,
            },
            "counts": counts,
            "state_changes": {
                "pipelines": pipelines,
                "bind_groups": bind_groups,
                "vertex_buffers": vertex_buffers,
                "index_buffers": index_buffers,
            },
            "command_buffers": self.command_buffers,
            "upload_bytes_per_frame": uploads,
            "reallocations": self.reallocations,
            "instance_batches": batches,
            "memory": parsed(&self.memory_json),
            "allocations_per_frame": round(self.allocs_per_frame),
            "allocation_bytes_per_frame": round(self.alloc_bytes_per_frame),
        })
    }
}

/// A whole run of the scene suite.
#[derive(Clone, Debug, Default)]
pub struct RenderReport {
    /// `SceneBench::gpu_json`: the adapter, limits, features, tier and target.
    pub gpu_json: String,
    pub frames: usize,
    pub warmup: usize,
    pub size: (u32, u32),
    pub samples: u32,
    pub quality: String,
    pub scale: f32,
    /// The process's largest resident set after the run, in bytes.
    pub peak_rss_bytes: Option<u64>,
    /// Why `peak_rss_bytes` is missing, when it is.
    pub peak_rss_unavailable: Option<String>,
    pub scenes: Vec<SceneReport>,
}

impl RenderReport {
    /// The `render` section of the benchmark JSON document.
    pub fn to_json(&self) -> String {
        let scenes: Vec<Value> = self.scenes.iter().map(SceneReport::json).collect();
        let value = json!({
            "schema_version": RENDER_SCHEMA_VERSION,
            "settings": {
                "frames": self.frames,
                "warmup_frames": self.warmup,
                "width": self.size.0,
                "height": self.size.1,
                "samples": self.samples,
                "quality": self.quality,
                "scale": self.scale,
                "presentation": "offscreen: no window, no display wait; every frame waits for the GPU",
            },
            "gpu": serde_json::from_str::<Value>(&self.gpu_json).unwrap_or(Value::Null),
            "process": {
                "peak_rss_bytes": self.peak_rss_bytes,
                "peak_rss_unavailable": self.peak_rss_unavailable,
            },
            "scenes": scenes,
        });
        serde_json::to_string_pretty(&value).expect("a JSON value serializes")
    }

    /// One row per scene and metric: the spread of every timing, GPU pass
    /// and count, the mean uploads and the counted memory. A metric that
    /// couldn't be measured has empty numbers and its reason in `note`.
    pub fn summary_csv(&self) -> String {
        let mut out = String::from("scene,group,metric,unit,samples,min,p50,p95,p99,max,mean,note\n");
        let mut dist = |scene: &str, group: &str, metric: &str, unit: &str, d: Option<Dist>, note: &str| {
            let _ = match d {
                Some(d) => writeln!(
                    out,
                    "{scene},{group},{},{unit},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{}",
                    csv(metric),
                    d.samples,
                    d.min,
                    d.p50,
                    d.p95,
                    d.p99,
                    d.max,
                    d.mean,
                    csv(note)
                ),
                None => writeln!(out, "{scene},{group},{},{unit},0,,,,,,,{}", csv(metric), csv(note)),
            };
        };
        for scene in &self.scenes {
            let name = scene.name.as_str();
            for (metric, _) in TIMINGS {
                dist(name, "cpu", metric, "ms", scene.timing(metric), "");
            }
            let why = scene.gpu.unavailable.as_deref().unwrap_or("");
            dist(name, "gpu", "total", "ms", scene.gpu.total, why);
            dist(name, "gpu", "span", "ms", scene.gpu.span, why);
            for (pass, d) in &scene.gpu.passes {
                dist(name, "gpu_pass", pass, "ms", Some(*d), "");
            }
            for (metric, get) in COUNTS {
                dist(name, "count", metric, "count", scene.count(get), "");
            }
            let one = |v: f64| {
                Some(Dist {
                    samples: 1,
                    min: v,
                    p50: v,
                    p95: v,
                    p99: v,
                    max: v,
                    mean: v,
                })
            };
            for &(kind, bytes) in &scene.uploads_per_frame {
                dist(
                    name,
                    "upload",
                    kind,
                    "bytes/frame",
                    one(bytes),
                    "mean over the measured frames",
                );
            }
            dist(
                name,
                "alloc",
                "allocations",
                "count/frame",
                one(scene.allocs_per_frame),
                "whole process",
            );
            dist(
                name,
                "alloc",
                "bytes",
                "bytes/frame",
                one(scene.alloc_bytes_per_frame),
                "whole process",
            );
            let memory = serde_json::from_str::<Value>(&scene.memory_json).unwrap_or(Value::Null);
            for key in [
                "mesh_bytes",
                "gpu_buffer_bytes",
                "instance_batch_pool_bytes",
                "texture_bytes",
                "counted_bytes",
            ] {
                dist(name, "memory", key, "bytes", memory[key].as_f64().and_then(one), "");
            }
            // Absent in files recorded before it was reported: then no row.
            if let Some(bytes) = memory["mesh_cpu_bytes"].as_f64() {
                dist(name, "memory", "mesh_cpu_bytes", "bytes", one(bytes), "on the CPU");
            }
            // Absent in files recorded before these were computed: then no row.
            for key in [
                "render_target_bytes",
                "shadow_map_bytes",
                "light_shadow_atlas_bytes",
                "computed_bytes",
            ]
            .into_iter()
            .filter(|key| memory[*key].is_number())
            {
                dist(
                    name,
                    "memory",
                    key,
                    "bytes",
                    memory[key].as_f64().and_then(one),
                    "computed from texture sizes, not read from the device",
                );
            }
            let device_note = memory["device_unavailable"].as_str().unwrap_or("");
            for key in ["device_allocated_bytes", "device_reserved_bytes"] {
                dist(
                    name,
                    "memory",
                    key,
                    "bytes",
                    memory[key].as_f64().and_then(one),
                    device_note,
                );
            }
        }
        let rss = self.peak_rss_bytes.map(|b| b as f64).map(|v| Dist {
            samples: 1,
            min: v,
            p50: v,
            p95: v,
            p99: v,
            max: v,
            mean: v,
        });
        dist(
            "all",
            "memory",
            "peak_rss_bytes",
            "bytes",
            rss,
            self.peak_rss_unavailable.as_deref().unwrap_or("the whole run"),
        );
        out
    }

    /// One row per measured frame of every scene. GPU columns are empty
    /// for frames that weren't timed.
    pub fn frames_csv(&self) -> String {
        let mut out = String::from(
            "scene,frame,update_ms,collect_ms,prepare_ms,encode_ms,submit_ms,cpu_ms,frame_ms,gpu_ms,gpu_span_ms,\
             draw_calls,recorded_draws,instances,triangles,shadow_instances,shadow_triangles,occluded,upload_bytes,\
             buffers_created,pipelines_built\n",
        );
        let optional = |v: Option<f64>| v.map_or_else(String::new, |v| format!("{v:.6}"));
        for scene in &self.scenes {
            for (i, f) in scene.frames.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "{},{i},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{},{},{},{},{},{},{},{}",
                    scene.name,
                    f.update_ms,
                    f.collect_ms,
                    f.prepare_ms,
                    f.encode_ms,
                    f.submit_ms,
                    f.cpu_ms(),
                    f.frame_ms,
                    optional(f.gpu_ms),
                    optional(f.gpu_span_ms),
                    f.draw_calls,
                    f.recorded_draws,
                    f.instances,
                    f.triangles,
                    f.shadow_instances,
                    f.shadow_triangles,
                    f.occluded,
                    f.upload_bytes,
                    f.buffers_created,
                    f.pipelines_built,
                );
            }
        }
        out
    }
}

/// A CSV field: quoted when it holds a comma, a quote or a line break.
fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// What must match for two runs' numbers to be compared, read from the
/// `render` sections of two benchmark documents: `Err` lists every
/// difference (another adapter or backend, resolution, sample count,
/// quality, scale, tier, or a scene whose content or settings differ).
pub fn comparable(base: &Value, new: &Value) -> Result<(), Vec<String>> {
    let mut differences = Vec::new();
    let mut check = |what: &str, a: &Value, b: &Value| {
        if a != b {
            differences.push(format!("{what}: {a} against {b}"));
        }
    };
    let (a, b) = (&base["render"], &new["render"]);
    for key in [
        "width",
        "height",
        "samples",
        "quality",
        "scale",
        "frames",
        "warmup_frames",
    ] {
        check(key, &a["settings"][key], &b["settings"][key]);
    }
    for key in ["name", "backend", "driver", "driver_info"] {
        check(
            &format!("adapter {key}"),
            &a["gpu"]["capabilities"]["adapter"][key],
            &b["gpu"]["capabilities"]["adapter"][key],
        );
    }
    check(
        "GPU tier",
        &a["gpu"]["capabilities"]["tier"],
        &b["gpu"]["capabilities"]["tier"],
    );
    check(
        "samples used",
        &a["gpu"]["target"]["samples"],
        &b["gpu"]["target"]["samples"],
    );
    check("os", &base["env"]["os"], &new["env"]["os"]);
    check("cpu", &base["env"]["cpu"], &new["env"]["cpu"]);
    check(
        "debug assertions",
        &base["env"]["debug_assertions"],
        &new["env"]["debug_assertions"],
    );
    let scenes = |doc: &Value| doc["scenes"].as_array().cloned().unwrap_or_default();
    let (ours, theirs) = (scenes(a), scenes(b));
    let mut missing = Vec::new();
    for scene in &ours {
        let name = &scene["name"];
        match theirs.iter().find(|s| &s["name"] == name) {
            Some(other) => check(&format!("scene {name}"), &scene["scene"], &other["scene"]),
            None => missing.push(format!("scene {name}: only in the base run")),
        }
    }
    differences.append(&mut missing);
    for scene in &theirs {
        let name = &scene["name"];
        if !ours.iter().any(|s| &s["name"] == name) {
            differences.push(format!("scene {name}: only in the new run"));
        }
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences)
    }
}

/// What `render_compare` prints for two runs: the table of
/// [`compare_table`] when they measured the same thing, otherwise only the
/// differences ([`comparable`]) and no table, so numbers that can't be
/// compared are never set side by side.
pub fn compare(base: &Value, new: &Value) -> Result<String, Vec<String>> {
    comparable(base, new)?;
    Ok(compare_table(base, new))
}

/// The medians of two runs side by side, as a Markdown table: every scene
/// both ran, its CPU steps, frame time and GPU total. The change column is
/// the new median relative to the base. It says nothing about which
/// difference is noise: read the 95th percentiles and the load averages.
pub fn compare_table(base: &Value, new: &Value) -> String {
    let mut out = String::from("| scene | metric | base p50 ms | new p50 ms | change | base p95 ms | new p95 ms |\n");
    out.push_str("|---|---|---:|---:|---:|---:|---:|\n");
    let scenes = |doc: &Value| doc["render"]["scenes"].as_array().cloned().unwrap_or_default();
    let others = scenes(new);
    for scene in scenes(base) {
        let Some(other) = others.iter().find(|s| s["name"] == scene["name"]) else {
            continue;
        };
        let name = scene["name"].as_str().unwrap_or("?");
        let mut row = |metric: &str, a: &Value, b: &Value| {
            let number = |v: &Value| v.as_f64().map_or_else(|| "n/a".to_owned(), |v| format!("{v:.3}"));
            let change = match (a["p50"].as_f64(), b["p50"].as_f64()) {
                (Some(x), Some(y)) if x > 0.0 => format!("{:+.1}%", (y / x - 1.0) * 100.0),
                _ => "n/a".to_owned(),
            };
            let _ = writeln!(
                out,
                "| {name} | {metric} | {} | {} | {change} | {} | {} |",
                number(&a["p50"]),
                number(&b["p50"]),
                number(&a["p95"]),
                number(&b["p95"]),
            );
        };
        for (metric, _) in TIMINGS {
            row(metric, &scene["timings_ms"][metric], &other["timings_ms"][metric]);
        }
        row("gpu total", &scene["gpu_ms"]["total"], &other["gpu_ms"]["total"]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ms: f64, gpu: Option<f64>) -> FrameRow {
        FrameRow {
            update_ms: 0.5,
            collect_ms: ms,
            prepare_ms: ms * 2.0,
            encode_ms: ms * 3.0,
            submit_ms: ms * 4.0,
            frame_ms: ms * 20.0,
            gpu_ms: gpu,
            gpu_span_ms: gpu.map(|g| g + 1.0),
            draw_calls: 7,
            triangles: 1200,
            upload_bytes: 256,
            ..Default::default()
        }
    }

    fn scene(name: &str, timed: bool) -> SceneReport {
        let frames: Vec<FrameRow> = (1..=4)
            .map(|i| row(f64::from(i), timed.then_some(f64::from(i))))
            .collect();
        let gpu = if timed {
            let d = Dist::of(&mut [1.0, 2.0, 3.0, 4.0]).unwrap();
            GpuTimes {
                timed_frames: 4,
                total: Some(d),
                span: Some(d),
                passes: vec![("opaque".into(), d), ("post, bloom".into(), d)],
                unavailable: None,
            }
        } else {
            GpuTimes {
                unavailable: Some("the adapter (Software, dx12) has no timestamp queries".into()),
                ..Default::default()
            }
        };
        SceneReport {
            name: name.into(),
            scene_json: r#"{"name":"x","content":{"models":3},"quality":{"level":"high"}}"#.into(),
            memory_json: r#"{"mesh_bytes":1024,"gpu_buffer_bytes":0,"instance_batch_pool_bytes":0,"texture_bytes":4,"counted_bytes":1028,"device_allocated_bytes":null,"device_reserved_bytes":null,"device_unavailable":"wgpu reports no allocator totals on the metal backend"}"#.into(),
            warmup: 2,
            frames,
            gpu,
            uploads_per_frame: vec![("instances", 128.0), ("uniforms", 64.5)],
            instance_batches: vec![("batches", 2)],
            state_changes: [1, 2, 3, 4],
            command_buffers: 7,
            allocs_per_frame: 1.5,
            alloc_bytes_per_frame: 96.0,
            ..Default::default()
        }
    }

    fn report(timed: bool) -> RenderReport {
        RenderReport {
            gpu_json: r#"{"capabilities":{"adapter":{"name":"A","backend":"metal","driver":"","driver_info":""},"tier":"desktop"},"target":{"samples":4}}"#.into(),
            frames: 4,
            warmup: 2,
            size: (1280, 720),
            samples: 4,
            quality: "high".into(),
            scale: 1.0,
            peak_rss_bytes: timed.then_some(1 << 20),
            peak_rss_unavailable: (!timed).then(|| "not read on this platform".to_owned()),
            scenes: vec![scene("static-world", timed), scene("animated", timed)],
        }
    }

    fn document(report: &RenderReport) -> Value {
        json!({
            "env": {"os": "macos", "cpu": "C", "debug_assertions": false},
            "render": serde_json::from_str::<Value>(&report.to_json()).unwrap(),
        })
    }

    #[test]
    fn distributions_use_nearest_rank_percentiles() {
        let mut values: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let d = Dist::of(&mut values).unwrap();
        assert_eq!(
            (d.samples, d.min, d.p50, d.p95, d.p99, d.max),
            (100, 1.0, 50.0, 95.0, 99.0, 100.0)
        );
        assert_eq!(d.mean, 50.5);
        let one = Dist::of(&mut [3.0]).unwrap();
        assert_eq!((one.min, one.p50, one.p99, one.max), (3.0, 3.0, 3.0, 3.0));
        assert_eq!(Dist::of(&mut []), None);
    }

    #[test]
    fn the_json_section_has_every_scene_with_its_timings_counts_and_memory() {
        let doc: Value = serde_json::from_str(&report(true).to_json()).unwrap();
        assert_eq!(doc["schema_version"], RENDER_SCHEMA_VERSION);
        assert_eq!(doc["settings"]["width"], 1280);
        assert_eq!(doc["settings"]["samples"], 4);
        assert_eq!(doc["gpu"]["capabilities"]["adapter"]["backend"], "metal");
        assert_eq!(doc["process"]["peak_rss_bytes"], 1 << 20);
        let scenes = doc["scenes"].as_array().unwrap();
        assert_eq!(scenes.len(), 2);
        let s = &scenes[0];
        assert_eq!(s["name"], "static-world");
        assert_eq!(s["frames"], 4);
        assert_eq!(s["scene"]["content"]["models"], 3);
        for step in ["update", "collect", "prepare", "encode", "submit", "cpu", "frame"] {
            assert_eq!(s["timings_ms"][step]["samples"], 4, "{step}");
        }
        // cpu is the four renderer steps added up, frame by frame.
        assert_eq!(s["timings_ms"]["cpu"]["max"], 40.0);
        assert_eq!(s["timings_ms"]["collect"]["p50"], 2.0);
        assert_eq!(s["gpu_ms"]["timed_frames"], 4);
        assert_eq!(s["gpu_ms"]["passes"][1]["name"], "post, bloom");
        assert_eq!(s["gpu_ms"]["unavailable"], Value::Null);
        assert_eq!(s["counts"]["triangles"]["p50"], 1200.0);
        assert_eq!(s["state_changes"]["bind_groups"], 2);
        assert_eq!(s["upload_bytes_per_frame"]["uniforms"], 64.5);
        assert_eq!(s["memory"]["mesh_bytes"], 1024);
        assert_eq!(s["allocations_per_frame"], 1.5);
    }

    #[test]
    fn missing_gpu_times_and_memory_are_null_with_a_reason_never_zero() {
        let doc: Value = serde_json::from_str(&report(false).to_json()).unwrap();
        let gpu = &doc["scenes"][0]["gpu_ms"];
        assert_eq!(gpu["total"], Value::Null);
        assert_eq!(gpu["span"], Value::Null);
        assert_eq!(gpu["timed_frames"], 0);
        assert!(gpu["passes"].as_array().unwrap().is_empty());
        assert!(gpu["unavailable"].as_str().unwrap().contains("no timestamp queries"));
        assert_eq!(doc["process"]["peak_rss_bytes"], Value::Null);
        assert_eq!(doc["process"]["peak_rss_unavailable"], "not read on this platform");
        assert_eq!(doc["scenes"][0]["memory"]["device_allocated_bytes"], Value::Null);

        let csv = report(false).summary_csv();
        let line = |start: &str| {
            csv.lines()
                .find(|l| l.starts_with(start))
                .unwrap_or_else(|| panic!("{start}"))
        };
        // Empty numbers, zero samples, and the reason (quoted: it has a comma).
        assert_eq!(
            line("static-world,gpu,total"),
            "static-world,gpu,total,ms,0,,,,,,,\"the adapter (Software, dx12) has no timestamp queries\""
        );
        assert!(
            line("static-world,memory,device_allocated_bytes").ends_with("no allocator totals on the metal backend")
        );
        assert_eq!(
            line("all,memory,peak_rss_bytes"),
            "all,memory,peak_rss_bytes,bytes,0,,,,,,,not read on this platform"
        );
    }

    #[test]
    fn the_summary_csv_has_one_row_per_scene_and_metric_and_as_many_columns_as_its_header() {
        let csv = report(true).summary_csv();
        let mut lines = csv.lines();
        let header = lines.next().unwrap();
        assert_eq!(header, "scene,group,metric,unit,samples,min,p50,p95,p99,max,mean,note");
        let columns = header.split(',').count();
        for line in lines.clone() {
            // No quoted commas except in the pass name and notes checked below.
            if !line.contains('"') {
                assert_eq!(line.split(',').count(), columns, "{line}");
            }
        }
        assert!(csv.contains("static-world,cpu,collect,ms,4,1.000000,2.000000,4.000000,4.000000,4.000000,2.500000,\n"));
        assert!(csv.contains("animated,gpu_pass,\"post, bloom\",ms,4,"));
        assert!(csv.contains("animated,count,draw_calls,count,4,7.000000"));
        assert!(csv.contains("static-world,upload,instances,bytes/frame,1,128.000000"));
        assert!(csv.contains("static-world,memory,counted_bytes,bytes,1,1028.000000"));
        assert!(csv.contains("all,memory,peak_rss_bytes,bytes,1,1048576.000000"));
        // 7 timings, 2 GPU totals, 2 passes, 10 counts, 2 uploads, 2 allocation rows, 7 memory rows.
        assert_eq!(lines.filter(|l| l.starts_with("animated,")).count(), 32);
    }

    #[test]
    fn the_frames_csv_has_a_row_per_frame_and_leaves_untimed_gpu_columns_empty() {
        let csv = report(true).frames_csv();
        let header = csv.lines().next().unwrap();
        assert!(header.starts_with("scene,frame,update_ms,collect_ms"));
        assert_eq!(csv.lines().count(), 1 + 2 * 4);
        for line in csv.lines() {
            assert_eq!(line.split(',').count(), header.split(',').count(), "{line}");
        }
        assert!(csv.contains(
            "static-world,1,0.500000,2.000000,4.000000,6.000000,8.000000,20.000000,40.000000,2.000000,3.000000,7,0,0,1200,0,0,0,256,0,0\n"
        ));
        let untimed = report(false).frames_csv();
        assert!(untimed.contains("animated,0,0.500000,1.000000,2.000000,3.000000,4.000000,10.000000,20.000000,,,7,"));
    }

    #[test]
    fn csv_fields_with_commas_or_quotes_are_quoted() {
        assert_eq!(csv("plain"), "plain");
        assert_eq!(csv("a, b"), "\"a, b\"");
        assert_eq!(csv("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn runs_of_the_same_setup_are_comparable_and_every_difference_is_named() {
        let base = document(&report(true));
        assert_eq!(comparable(&base, &base), Ok(()));

        let mut other = report(true);
        other.size = (1920, 1080);
        other.samples = 1;
        other.gpu_json = other.gpu_json.replace("\"A\"", "\"B\"");
        other.scenes[1].scene_json = other.scenes[1].scene_json.replace("\"models\":3", "\"models\":4");
        let mut new = document(&other);
        new["env"]["cpu"] = json!("D");
        let differences = comparable(&base, &new).unwrap_err();
        for expected in [
            "width",
            "height",
            "samples",
            "adapter name",
            "cpu",
            "scene \"animated\"",
        ] {
            assert!(
                differences.iter().any(|d| d.starts_with(expected)),
                "{expected} in {differences:?}"
            );
        }
        assert!(!differences.iter().any(|d| d.starts_with("scene \"static-world\"")));
    }

    #[test]
    fn runs_of_different_length_or_with_different_scenes_are_not_comparable() {
        let base = document(&report(true));
        let mut other = report(true);
        other.frames = 8;
        other.warmup = 0;
        other.scenes.remove(0);
        other.scenes.push(scene("game-like", true));
        let differences = comparable(&base, &document(&other)).unwrap_err();
        for expected in [
            "frames: 4 against 8",
            "warmup_frames: 2 against 0",
            "scene \"static-world\": only in the base run",
            "scene \"game-like\": only in the new run",
        ] {
            assert!(
                differences.iter().any(|d| d == expected),
                "{expected} in {differences:?}"
            );
        }
        assert!(!differences.iter().any(|d| d.starts_with("scene \"animated\"")));
    }

    #[test]
    fn runs_that_are_not_comparable_get_their_differences_and_no_table() {
        let base = document(&report(true));
        let table = compare(&base, &base).unwrap();
        assert!(
            table.contains("| static-world | collect | 2.000 | 2.000 | +0.0% |"),
            "{table}"
        );

        let mut other = report(true);
        other.size = (1920, 1080);
        let refused = compare(&base, &document(&other)).unwrap_err();
        assert!(refused.iter().any(|d| d.starts_with("width")), "{refused:?}");
        assert!(refused.iter().all(|d| !d.contains('|')), "no table rows: {refused:?}");
    }

    #[test]
    fn the_comparison_table_lists_both_medians_and_marks_missing_gpu_times() {
        let base = document(&report(true));
        let mut slower = report(true);
        for scene in &mut slower.scenes {
            for frame in &mut scene.frames {
                frame.collect_ms *= 1.5;
            }
        }
        let table = compare_table(&base, &document(&slower));
        assert!(
            table.contains("| static-world | collect | 2.000 | 3.000 | +50.0% | 4.000 | 6.000 |"),
            "{table}"
        );
        assert!(table.contains("| animated | prepare | 4.000 | 4.000 | +0.0% |"));
        let table = compare_table(&base, &document(&report(false)));
        assert!(
            table.contains("| static-world | gpu total | 2.000 | n/a | n/a | 4.000 | n/a |"),
            "{table}"
        );
    }

    /// The committed baselines, as checked out (a Windows checkout may have
    /// turned their line ends into CRLF).
    fn baseline(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    const SCENES: [&str; 8] = [
        "single-material",
        "static-world",
        "dynamic-world",
        "multi-material",
        "shadow-heavy",
        "alpha-heavy",
        "animated",
        "game-like",
    ];

    #[test]
    fn the_committed_baseline_has_every_scene_and_every_metric_or_a_reason() {
        let doc: Value = serde_json::from_str(&baseline(include_str!("../baselines/render.json"))).unwrap();
        let render = &doc["render"];
        assert_eq!(render["schema_version"], RENDER_SCHEMA_VERSION);
        let names: Vec<&str> = render["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, SCENES);
        for key in [
            "width",
            "height",
            "samples",
            "quality",
            "scale",
            "frames",
            "warmup_frames",
        ] {
            assert!(!render["settings"][key].is_null(), "settings.{key}");
        }
        let adapter = &render["gpu"]["capabilities"]["adapter"];
        assert!(!adapter["name"].as_str().unwrap().is_empty());
        assert!(!adapter["backend"].as_str().unwrap().is_empty());
        assert!(render["gpu"]["capabilities"]["limits"].is_object());
        assert!(!render["gpu"]["capabilities"]["features"].as_array().unwrap().is_empty());
        for scene in render["scenes"].as_array().unwrap() {
            let name = &scene["name"];
            for step in ["collect", "prepare", "encode", "submit", "cpu", "frame"] {
                for key in ["min", "p50", "p95", "p99", "max", "mean"] {
                    assert!(scene["timings_ms"][step][key].is_number(), "{name} {step} {key}");
                }
            }
            // GPU times, or the reason there are none.
            let gpu = &scene["gpu_ms"];
            assert!(
                gpu["total"]["p50"].is_number() && !gpu["passes"].as_array().unwrap().is_empty()
                    || gpu["unavailable"].is_string(),
                "{name}: {gpu}"
            );
            assert!(scene["counts"]["draw_calls"]["p50"].is_number(), "{name}");
            // Memory: what is counted, and what is not said in words.
            let memory = &scene["memory"];
            assert!(memory["counted_bytes"].is_number(), "{name}");
            assert!(memory["uncounted"].is_string(), "{name}");
            assert!(
                memory["device_allocated_bytes"].is_number() || memory["device_unavailable"].is_string(),
                "{name}: {memory}"
            );
            assert!(scene["scene"]["quality"]["shadow_resolution"].is_number(), "{name}");
            if name == "shadow-heavy" {
                // The benchmark's own shadow maps (the engine's tests draw smaller ones).
                assert_eq!(scene["scene"]["quality"]["shadow_resolution"], 4096);
            }
        }
        assert!(
            render["process"]["peak_rss_bytes"].is_number() || render["process"]["peak_rss_unavailable"].is_string()
        );
        assert_eq!(comparable(&doc, &doc), Ok(()));
        // The same numbers as records, one per step, like every other benchmark.
        let results = doc["results"].as_array().unwrap();
        for scene in SCENES {
            let record = format!("render/scene {scene} frame");
            assert!(results.iter().any(|r| r["name"] == record.as_str()), "{record}");
        }
    }

    #[test]
    fn the_committed_csv_files_share_one_header() {
        let header = "scene,group,metric,unit,samples,min,p50,p95,p99,max,mean,note";
        let ours = baseline(include_str!("../baselines/render.csv"));
        assert_eq!(ours.lines().next(), Some(header));
        for scene in SCENES {
            assert!(ours.contains(&format!("\n{scene},cpu,frame,ms,")), "{scene}");
        }
        for other in [
            baseline(include_str!("../baselines/render-matched.csv")),
            baseline(include_str!("../baselines/render-bevy.csv")),
        ] {
            assert_eq!(other.lines().next(), Some(header));
            // The three scenes the matched and Bevy runs draw.
            for scene in &SCENES[1..4] {
                assert!(other.contains(&format!("\n{scene},cpu,frame,ms,")), "{scene}");
            }
        }
    }

    #[test]
    fn the_matched_baseline_is_not_compared_with_the_plain_one() {
        let plain: Value = serde_json::from_str(&baseline(include_str!("../baselines/render.json"))).unwrap();
        let matched: Value = serde_json::from_str(&baseline(include_str!("../baselines/render-matched.json"))).unwrap();
        assert_eq!(matched["render"]["settings"]["quality"], "high (matched)");
        let differences = comparable(&plain, &matched).unwrap_err();
        assert!(differences.iter().any(|d| d.starts_with("quality")), "{differences:?}");
        // The other engine's run says why it has no GPU times.
        let bevy: Value = serde_json::from_str(&baseline(include_str!("../baselines/render-bevy.json"))).unwrap();
        for scene in bevy["scenes"].as_array().unwrap() {
            assert!(scene["gpu"]["unavailable"].is_string(), "{scene}");
            assert!(scene["timings_ms"]["frame"]["p50"].is_number(), "{scene}");
        }
    }
}
