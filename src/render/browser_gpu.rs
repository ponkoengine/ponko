//! Opening the GPU in a browser: WebGPU first, WebGL2 where WebGPU is
//! missing or doesn't work.
//!
//! wgpu decides between the two when its instance is made (an instance is
//! either the browser's WebGPU or wgpu's own WebGL2 backend), and a canvas
//! keeps the first kind of context it hands out. So the WebGPU path asks
//! for an adapter and a device, and checks that the device survived its
//! first round trip to the GPU, before it touches the canvas. Anything that
//! goes wrong before then leaves the canvas free for WebGL2, logs one line
//! (`ponko: falling back to WebGL2: <why>`) and starts over on WebGL2.

/// Why a browser build draws with WebGL2 instead of WebGPU.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WebGpuFallback {
    /// The page has no `navigator.gpu`.
    Missing,
    /// `navigator.gpu.requestAdapter()` gave no adapter.
    NoAdapter,
    /// The adapter refused to open a device (the browser's words).
    DeviceFailed(String),
    /// The device was lost before its first submission finished (the
    /// browser's words).
    LostAtStart(String),
    /// The canvas gave no WebGPU context (the browser's words).
    Surface(String),
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl WebGpuFallback {
    /// The one line logged in the browser's console when the game falls back.
    pub(super) fn log_line(&self) -> String {
        let why = match self {
            WebGpuFallback::Missing => "this browser has no WebGPU (navigator.gpu is missing)".to_owned(),
            WebGpuFallback::NoAdapter => "navigator.gpu.requestAdapter() found no adapter".to_owned(),
            WebGpuFallback::DeviceFailed(e) => format!("the WebGPU adapter could not open a device: {}", words(e)),
            WebGpuFallback::LostAtStart(e) => format!("the WebGPU device was lost as it opened: {}", words(e)),
            WebGpuFallback::Surface(e) => format!("the canvas gave no WebGPU context: {}", words(e)),
        };
        format!("ponko: falling back to WebGL2: {why}")
    }
}

/// The browser's message on one line, or a stand-in when it gave none. A
/// JavaScript error reaches here as wgpu prints it,
/// `JsValue(OperationError: <message>\n<stack>)`: the message's first line
/// is kept, without the wrapper and the stack.
fn words(message: &str) -> String {
    let message = message.trim();
    let message = message
        .strip_prefix("JsValue(")
        .map_or(message, |inner| inner.strip_suffix(')').unwrap_or(inner));
    let line = message.lines().next().unwrap_or_default().trim().to_owned();
    if line.is_empty() {
        "the browser gave no reason".to_owned()
    } else {
        line
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) use web::{Opened, open};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::WebGpuFallback;
    use crate::error::Error;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use wasm_bindgen::JsValue;
    use winit::window::Window;

    /// A GPU opened for the game's window.
    pub(in crate::render) struct Opened {
        pub(in crate::render) instance: wgpu::Instance,
        pub(in crate::render) surface: wgpu::Surface<'static>,
        pub(in crate::render) adapter: wgpu::Adapter,
        pub(in crate::render) device: wgpu::Device,
        pub(in crate::render) queue: wgpu::Queue,
    }

    /// Opens WebGPU where the browser has a working one, else WebGL2, and
    /// logs which (one line). `device_descriptor` gives the device to ask
    /// an adapter for.
    pub(in crate::render) async fn open(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        device_descriptor: &dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>,
    ) -> Result<Opened, Error> {
        match webgpu(&window, display.clone(), device_descriptor).await {
            Ok(opened) => {
                web_sys::console::info_1(&JsValue::from_str("ponko: drawing with WebGPU"));
                return Ok(opened);
            }
            Err(why) => web_sys::console::warn_1(&JsValue::from_str(&why.log_line())),
        }
        webgl2(window, display, device_descriptor).await
    }

    fn instance(display: winit::event_loop::OwnedDisplayHandle, backends: wgpu::Backends) -> wgpu::Instance {
        let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display));
        descriptor.backends = backends;
        wgpu::Instance::new(descriptor)
    }

    async fn webgpu(
        window: &Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        device_descriptor: &dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>,
    ) -> Result<Opened, WebGpuFallback> {
        if !has_navigator_gpu() {
            return Err(WebGpuFallback::Missing);
        }
        let instance = instance(display, wgpu::Backends::BROWSER_WEBGPU);
        // A WebGPU adapter draws to any canvas: no surface yet, so the
        // canvas stays free for WebGL2 until the device is known to work.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                ..Default::default()
            })
            .await
            .map_err(|_| WebGpuFallback::NoAdapter)?;
        let (device, queue) = adapter
            .request_device(&device_descriptor(&adapter))
            .await
            .map_err(|e| WebGpuFallback::DeviceFailed(e.to_string()))?;
        // A device the browser loses right away (its GPU process is gone,
        // the driver refused) can't draw: wait for one round trip to the
        // GPU and see whether it is still there.
        let lost = Arc::new(Mutex::new(None::<String>));
        let lost_in_callback = lost.clone();
        device.set_device_lost_callback(move |_, message| {
            if let Ok(mut lost) = lost_in_callback.lock() {
                lost.get_or_insert(message);
            }
        });
        let done = Arc::new(AtomicBool::new(false));
        let done_in_callback = done.clone();
        queue.on_submitted_work_done(move || done_in_callback.store(true, Ordering::Relaxed));
        let lost_message = || lost.lock().ok().and_then(|lost| lost.clone());
        // At most about a second: the round trip itself takes a frame or less.
        for _ in 0..250 {
            if done.load(Ordering::Relaxed) || lost_message().is_some() {
                break;
            }
            next_task().await;
        }
        // The lost promise may settle right after the work-done one.
        next_task().await;
        if let Some(message) = lost_message() {
            return Err(WebGpuFallback::LostAtStart(message));
        }
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| WebGpuFallback::Surface(e.to_string()))?;
        Ok(Opened {
            instance,
            surface,
            adapter,
            device,
            queue,
        })
    }

    async fn webgl2(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        device_descriptor: &dyn Fn(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>,
    ) -> Result<Opened, Error> {
        use super::super::startup;
        let instance = instance(display, wgpu::Backends::GL);
        let surface = startup::result(instance.create_surface(window), "could not create a GPU surface")?;
        let adapter = startup::result(
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    ..Default::default()
                })
                .await,
            "no compatible GPU found",
        )?;
        let (device, queue) = startup::result(
            adapter.request_device(&device_descriptor(&adapter)).await,
            "could not open the GPU device",
        )?;
        Ok(Opened {
            instance,
            surface,
            adapter,
            device,
            queue,
        })
    }

    /// Whether the page has `navigator.gpu` (wgpu's own check isn't public).
    fn has_navigator_gpu() -> bool {
        web_sys::window()
            .and_then(|window| js_sys::Reflect::get(&window.navigator(), &JsValue::from_str("gpu")).ok())
            .is_some_and(|gpu| !gpu.is_undefined() && !gpu.is_null())
    }

    /// Waits for the browser's next task (a zero-delay timer), so promises
    /// settled meanwhile have run their callbacks.
    async fn next_task() {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            let scheduled = web_sys::window().is_some_and(|window| window.set_timeout_with_callback(&resolve).is_ok());
            if !scheduled {
                let _ = resolve.call0(&JsValue::NULL);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}

#[cfg(test)]
mod tests {
    use super::WebGpuFallback;

    #[test]
    fn every_fallback_logs_one_line_that_names_webgl2_and_the_reason() {
        let cases = [
            (WebGpuFallback::Missing, "navigator.gpu is missing"),
            (WebGpuFallback::NoAdapter, "requestAdapter() found no adapter"),
            (
                WebGpuFallback::DeviceFailed("JsValue(OperationError: Not enough memory\nundefined)".into()),
                "could not open a device: OperationError: Not enough memory",
            ),
            (
                WebGpuFallback::DeviceFailed("Not enough memory\n    at requestDevice (game.js:1:2)".into()),
                "could not open a device: Not enough memory",
            ),
            (
                WebGpuFallback::LostAtStart("Device was destroyed.".into()),
                "lost as it opened: Device was destroyed.",
            ),
            (WebGpuFallback::Surface(String::new()), "the browser gave no reason"),
        ];
        for (fallback, reason) in cases {
            let line = fallback.log_line();
            assert!(line.starts_with("ponko: falling back to WebGL2: "), "{line}");
            assert!(line.contains(reason), "{line} should say {reason:?}");
            assert!(
                !line.contains('\n') && !line.contains("JsValue") && !line.contains(" at "),
                "{line:?}"
            );
        }
    }

    /// The lines of `shaders` (path, text) that call a sampling function
    /// after `||` or `&&`. Browsers' WGSL compilers (Chrome's Tint) reject
    /// `textureSample` in non-uniform control flow, and `a || f()` calls `f`
    /// only where `a` is false: such a line makes every pipeline of its
    /// module invalid on WebGPU, so a 3D scene draws black. naga accepts it,
    /// so no native test sees it.
    fn samples_after_a_short_circuit(shaders: &[(String, String)]) -> Vec<String> {
        // Each function's body: from `fn name(` at the start of a line to
        // the brace that closes it.
        let mut bodies = Vec::new();
        for (_, text) in shaders {
            let mut current: Option<(String, String, i32)> = None;
            for line in text.lines() {
                let code = line.split("//").next().unwrap();
                // `fn name(` or `@fragment fn name(`, at the start of a line.
                let declared = if code.starts_with('@') {
                    code.find(" fn ").map(|at| &code[at + 1..])
                } else {
                    Some(code)
                };
                if current.is_none()
                    && let Some(rest) = declared.and_then(|d| d.strip_prefix("fn "))
                {
                    let name = rest.split('(').next().unwrap().trim().to_owned();
                    current = Some((name, String::new(), 0));
                }
                if let Some((_, body, depth)) = &mut current {
                    body.push_str(code);
                    body.push('\n');
                    *depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
                    if *depth <= 0 && code.contains('}') {
                        let (name, body, _) = current.take().unwrap();
                        bodies.push((name, body));
                    }
                }
            }
        }
        let samples = |body: &str| {
            ["textureSample(", "textureSampleBias(", "textureSampleCompare("]
                .iter()
                .any(|call| body.contains(call))
        };
        let calls = |line: &str, name: &str| {
            line.match_indices(&format!("{name}("))
                .any(|(at, _)| at == 0 || !line[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_'))
        };
        // The functions that sample, directly or through another.
        let mut sampling: Vec<&str> = bodies
            .iter()
            .filter(|(_, b)| samples(b))
            .map(|(n, _)| n.as_str())
            .collect();
        loop {
            let more: Vec<&str> = bodies
                .iter()
                .filter(|(n, b)| !sampling.contains(&n.as_str()) && sampling.iter().any(|s| calls(b, s)))
                .map(|(n, _)| n.as_str())
                .collect();
            if more.is_empty() {
                break;
            }
            sampling.extend(more);
        }
        let mut found = Vec::new();
        for (path, text) in shaders {
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap();
                let Some(at) = code.find("||").into_iter().chain(code.find("&&")).min() else {
                    continue;
                };
                let after = &code[at..];
                if samples(after) || sampling.iter().any(|s| calls(after, s)) {
                    found.push(format!("{path}:{}: {}", n + 1, line.trim()));
                }
            }
        }
        found
    }

    #[test]
    fn no_shader_samples_a_texture_after_a_short_circuit() {
        let mut shaders = Vec::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "wgsl") {
                    shaders.push((path.display().to_string(), std::fs::read_to_string(path).unwrap()));
                }
            }
        }
        assert!(shaders.len() > 10, "found the renderer's shaders");
        let found = samples_after_a_short_circuit(&shaders);
        assert!(
            found.is_empty(),
            "sample before the condition instead (let x = ...; if a || x ...): {found:#?}"
        );
        // The line Chrome rejected, against the real shaders: the mesh
        // shaders' `surface_at` samples (through the material's textures).
        let mut with_rejected = shaders.clone();
        with_rejected.push((
            "rejected.wgsl".into(),
            "fn probe(in: VertexOut, front: bool) {\n    if !fade_keeps(in.fade, in.clip.xy) || surface_at(in, front).alpha < 0.5 {\n        discard;\n    }\n}\n".into(),
        ));
        let flagged = samples_after_a_short_circuit(&with_rejected);
        assert!(
            flagged.len() == 1 && flagged[0].starts_with("rejected.wgsl:2:"),
            "{flagged:?}"
        );
        // The same through a helper of its own.
        let rejected = "fn surface_at(uv: vec2<f32>) -> vec4<f32> {\n    return textureSample(t, s, uv);\n}\n\
                        fn fs(in: In) {\n    if !keeps(in) || surface_at(in.uv).a < 0.5 {\n        discard;\n    }\n}\n";
        let flagged = samples_after_a_short_circuit(&[("rejected.wgsl".into(), rejected.into())]);
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert!(flagged[0].starts_with("rejected.wgsl:5:"), "{flagged:?}");
        // Sampled before the branch, as the shaders do: fine.
        let fixed = rejected.replace(
            "    if !keeps(in) || surface_at(in.uv).a < 0.5 {",
            "    let a = surface_at(in.uv).a;\n    if !keeps(in) || a < 0.5 {",
        );
        assert!(samples_after_a_short_circuit(&[("fixed.wgsl".into(), fixed)]).is_empty());
    }
}
